//! Resolved per-window presentation.
//!
//! A window has exactly one [`ResolvedWindowPresentation`], and it is the only
//! authority for what that window presents:
//!
//! - `requested` is the business request. It decides what the host *asks* the
//!   platform and the surface for, and nothing else.
//! - `effective` is what the surface could actually negotiate. It decides the
//!   clear colour, what the program is told, and the native chrome.
//! - `chrome` is derived from `effective` when the presentation is resolved, so
//!   there is no later place that could re-derive it from the request and land
//!   on the other answer.
//!
//! The state this type exists to make unreachable is "the renderer thinks
//! `Solid`, the HWND still wears transparent chrome": a `Transparent` request
//! on a DX12 HWND surface negotiates `Opaque`, falls back to `Solid`, and the
//! window must lose the transparent frame policy in the same step.
//!
//! The presentation *target* is settled before the window is created, because
//! `WS_EX_NOREDIRECTIONBITMAP` is a creation-time ex-style bit nothing can set
//! durably afterwards. Everything below that says "before the window exists"
//! says it for this reason.

use nana_ui_platform::{WindowDescriptor, WindowSurfacePreference};
use nana_window::{MaterialEffect, MaterialFallback, MaterialOutcome, NonClientRenderingStrategy};

/// What a process needs from its GPU backend.
///
/// The backend, adapter and device are process-wide — every window shares one
/// of each — so a requirement that constrains the backend belongs here and
/// nowhere else. Which windows then present through a platform compositor
/// visual is the separate per-window question
/// ([`WindowSurfacePreference`](nana_ui_platform::WindowSurfacePreference)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuBackendPolicy {
    /// No window needs a compositor visual, so the backend is chosen freely.
    #[default]
    Plain,
    /// Ask for a backend that can present through the platform compositor.
    ///
    /// On Windows that narrows the process to DX12, because DirectComposition
    /// visuals are a DX12 surface target. It does not put any window on that
    /// path; a window asks for it with `WindowSurfacePreference`.
    CompositionCapable,
}

impl GpuBackendPolicy {
    pub const fn wants_composition(self) -> bool {
        matches!(self, Self::CompositionCapable)
    }
}

/// How the compositor blends a window's surface, as the surface negotiated
/// it. Variant names follow WGPU's.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceAlphaMode {
    Auto,
    /// The surface is shown opaque; its alpha is ignored.
    Opaque,
    /// Colour is premultiplied by alpha before the compositor sees it.
    PreMultiplied,
    /// The compositor multiplies colour by alpha itself.
    PostMultiplied,
    /// Whatever the platform's own surface setting is.
    Inherit,
}

impl SurfaceAlphaMode {
    pub(crate) const fn from_wgpu(mode: wgpu::CompositeAlphaMode) -> Self {
        match mode {
            wgpu::CompositeAlphaMode::Auto => Self::Auto,
            wgpu::CompositeAlphaMode::Opaque => Self::Opaque,
            wgpu::CompositeAlphaMode::PreMultiplied => Self::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied => Self::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit => Self::Inherit,
        }
    }
}

/// Whether this process can present a window through a platform compositor.
/// Settled once, before the first window exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompositionAvailability {
    Available,
    Unavailable,
}

impl CompositionAvailability {
    const fn available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// What a process running on `backend` can present. A device replacement
    /// can land on another backend, and the answer has to move with it.
    pub(crate) fn for_backend(policy: GpuBackendPolicy, backend: wgpu::Backend) -> Self {
        let composable = cfg!(target_os = "windows") && backend == wgpu::Backend::Dx12;
        if policy.wants_composition() && composable {
            Self::Available
        } else {
            Self::Unavailable
        }
    }
}

/// Where a window's frames reach the screen. Per window, not per process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowSurfaceTarget {
    /// An HWND (or platform-equivalent) swapchain.
    #[default]
    NativeWindow,
    /// A DirectComposition visual owned by the host's composition tree.
    Composition,
}

impl WindowSurfaceTarget {
    /// Whether the window must be created without a redirection bitmap.
    ///
    /// This belongs to the presentation path, not to the material:
    /// DirectComposition draws its visual *over* that bitmap, so a composed
    /// window must not have one.
    pub const fn composed(self) -> bool {
        matches!(self, Self::Composition)
    }
}

/// Why a window is not presenting through the target it asked for.
///
/// A window that asked for the plain native path always gets it, so this is
/// only ever set on a window that asked to be composed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceTargetFallback {
    /// No GPU backend in this process can present that way.
    BackendUnavailable,
    /// The backend was there, but the target's native objects could not be
    /// built for this window — the composition device, its target for the
    /// HWND, the visual, the visual's GPU surface, or the premultiplied alpha
    /// that surface has to negotiate.
    TargetUnavailable,
}

impl SurfaceTargetFallback {
    /// Diagnostic text for a log or a startup error.
    ///
    /// Not for the interface: which presentation path a window reached is a
    /// technical explanation, and a product surface that wants to say something
    /// about transparency says it from
    /// [`MaterialOutcome::status_label`](nana_window::MaterialOutcome::status_label).
    pub const fn label(self) -> &'static str {
        match self {
            Self::BackendUnavailable => "当前设备没有可用的合成后端",
            Self::TargetUnavailable => "合成目标创建失败，已改用普通窗口呈现",
        }
    }
}

/// Native non-client chrome a client-chrome window must be given.
///
/// Derived from the effective material only. `None` chrome (see
/// [`ResolvedWindowPresentation::chrome`]) means the host does not own this
/// window's non-client area at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeChromePolicy {
    /// DWM round clip and the 1px system stroke that comes with it.
    pub rounded_corners: bool,
    /// Whether DWM must render no non-client area for this window.
    pub suppress_non_client: bool,
    /// How that suppression is achieved. Only meaningful while
    /// `suppress_non_client` — an opaque client's own pixels hide the
    /// non-client area, so it needs neither.
    pub non_client_strategy: NonClientRenderingStrategy,
}

impl NativeChromePolicy {
    /// The policy an opaque client takes: DWM may round, stroke and frame an
    /// HWND whose surface fills it.
    pub const OPAQUE: Self = Self {
        rounded_corners: true,
        suppress_non_client: false,
        non_client_strategy: NonClientRenderingStrategy::StripFrameStyles,
    };

    /// The policy a transparent client takes: DWM must render no non-client
    /// area, because the client would show it through.
    pub const fn transparent(strategy: NonClientRenderingStrategy) -> Self {
        Self {
            rounded_corners: false,
            suppress_non_client: true,
            non_client_strategy: strategy,
        }
    }

    /// Whether this policy takes the Win32 frame style bits away, and with
    /// them Aero Snap, Snap Layouts, Alt+Space and the system window
    /// animations.
    pub const fn frameless(self) -> bool {
        self.suppress_non_client && self.non_client_strategy.strips_frame_styles()
    }

    /// Whether DWM may render this window's non-client area.
    ///
    /// Off only for the strategy that keeps the frame styles and asks DWM to
    /// stop instead. The stripping strategy leaves the policy alone, because it
    /// has already removed the bits DWM renders from; an opaque client leaves
    /// it alone because its own pixels cover the result.
    pub const fn non_client_rendering_enabled(self) -> bool {
        !(self.suppress_non_client && !self.non_client_strategy.strips_frame_styles())
    }

    /// Chrome for the material the window is *actually* presenting.
    ///
    /// Only `Transparent` leaves the HWND rectangle for DWM to show through.
    /// Mica and Acrylic also report `wants_transparent_surface()`, but they
    /// *are* DWM's non-client rendering, so they keep the round clip and the
    /// stroke.
    const fn for_material(effective: MaterialEffect, strategy: NonClientRenderingStrategy) -> Self {
        match effective {
            MaterialEffect::Transparent => Self::transparent(strategy),
            MaterialEffect::Solid
            | MaterialEffect::Vibrancy
            | MaterialEffect::Mica
            | MaterialEffect::Acrylic => Self::OPAQUE,
        }
    }
}

/// What one window presents, and the native chrome that presentation requires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedWindowPresentation {
    requested: MaterialEffect,
    effective: MaterialOutcome,
    alpha_mode: wgpu::CompositeAlphaMode,
    requested_target: WindowSurfaceTarget,
    surface_target: WindowSurfaceTarget,
    target_fallback: Option<SurfaceTargetFallback>,
    chrome: Option<NativeChromePolicy>,
    shadow: nana_ui_platform::WindowShadowOutcome,
}

impl ResolvedWindowPresentation {
    /// Resolves the final presentation from the request and what the surface
    /// answered.
    ///
    /// `applied` is the outcome the platform material layer reported for
    /// `requested`; `alpha_mode` and `backend` are the surface's answer. The
    /// demotion and the chrome derivation happen together here, on purpose:
    /// there is no way to obtain an effective material without also obtaining
    /// the chrome that matches it.
    pub(crate) fn resolve(
        settings: &WindowDescriptor,
        requested: MaterialEffect,
        applied: MaterialOutcome,
        alpha_mode: wgpu::CompositeAlphaMode,
        backend: wgpu::Backend,
        target: ResolvedSurfaceTarget,
        non_client: NonClientRenderingStrategy,
    ) -> Self {
        let effective = demote_unpresentable(applied, alpha_mode, backend);
        Self {
            // Nothing applied yet: the host records the platform's answer.
            shadow: crate::window_shadow::pending(crate::window_shadow::intent(
                settings,
                effective.effect == MaterialEffect::Transparent,
            )),
            requested,
            effective,
            alpha_mode,
            requested_target: target.requested,
            surface_target: target.resolved,
            target_fallback: target.fallback,
            // A window that keeps the system caption owns none of this; the
            // host must not touch its frame, corners or stroke.
            chrome: (!settings.system_caption)
                .then(|| NativeChromePolicy::for_material(effective.effect, non_client)),
        }
    }

    /// The presentation of a window that no longer exists: a plain opaque
    /// window, because nothing is being presented at all. Reporting the last
    /// live window's state here would be a second, stale authority.
    pub(crate) const fn closed() -> Self {
        Self {
            requested: MaterialEffect::Solid,
            effective: MaterialOutcome::chosen_solid(),
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            requested_target: WindowSurfaceTarget::NativeWindow,
            surface_target: WindowSurfaceTarget::NativeWindow,
            target_fallback: None,
            chrome: None,
            shadow: nana_ui_platform::WindowShadowOutcome::disabled(),
        }
    }

    /// What the platform applied for the window's shadow. `Pending` until
    /// the host has applied it to the shown window.
    pub const fn shadow(&self) -> nana_ui_platform::WindowShadowOutcome {
        self.shadow
    }

    pub(crate) fn record_shadow(&mut self, outcome: nana_ui_platform::WindowShadowOutcome) {
        self.shadow = outcome;
    }

    /// What the application asked for. Never decides chrome.
    pub const fn requested(&self) -> MaterialEffect {
        self.requested
    }

    /// What the window presents, with the fallback reason when the request
    /// could not be met.
    pub const fn effective(&self) -> MaterialOutcome {
        self.effective
    }

    /// What the surface negotiated. This is the answer `effective` was derived
    /// from, not an independent one.
    pub const fn alpha_mode(&self) -> SurfaceAlphaMode {
        SurfaceAlphaMode::from_wgpu(self.alpha_mode)
    }

    /// The path this window's frames actually reach the screen through. See
    /// [`Self::requested_target`] for what it asked for.
    pub const fn surface_target(&self) -> WindowSurfaceTarget {
        self.surface_target
    }

    /// The target this window asked to present through.
    pub const fn requested_target(&self) -> WindowSurfaceTarget {
        self.requested_target
    }

    /// Why this window is not presenting through [`Self::requested_target`],
    /// or `None` when it is.
    pub const fn target_fallback(&self) -> Option<SurfaceTargetFallback> {
        self.target_fallback
    }

    /// The target as resolved, ready to be carried into a later presentation.
    ///
    /// `required` is not carried: a window that is already open has passed that
    /// gate, and a later appearance change must not be able to close it.
    pub(crate) const fn resolved_target(&self) -> ResolvedSurfaceTarget {
        ResolvedSurfaceTarget {
            requested: self.requested_target,
            resolved: self.surface_target,
            fallback: self.target_fallback,
            required: false,
        }
    }

    /// The native chrome this presentation requires, or `None` when the host
    /// does not own the window's non-client area.
    pub const fn chrome(&self) -> Option<NativeChromePolicy> {
        self.chrome
    }

    /// Whether the effective material differs from the request, so the native
    /// material applied for the request has to be undone.
    ///
    /// A demoted `Transparent` request has already extended the DWM frame
    /// across the client; leaving that glass on a window that now reports
    /// `Solid` is the same inconsistency as leaving it transparent chrome.
    pub(crate) fn needs_material_reset(&self) -> bool {
        self.effective.effect != self.requested
    }
}

/// Whether this platform has a composition surface target at all.
///
/// Only Windows does (a DirectComposition visual). Everywhere else the
/// platform's own window surface is already composited by the system with
/// per-pixel alpha — a `CAMetalLayer` on macOS — so there is no second path
/// to ask for, and whether that surface can carry a transparent client is the
/// negotiated alpha mode's answer (see `demote_unpresentable`).
const PLATFORM_HAS_COMPOSITION_TARGET: bool = cfg!(target_os = "windows");

/// The presentation target one window ends up asking for.
///
/// `Auto` only reaches the compositor when the process asked for a
/// compositor-capable backend *and* this window wants a transparent client:
/// the path exists for windows whose pixels need it, and an ordinary Settings
/// window, dialog or popup in the same process stays on the platform's own
/// window surface.
///
/// On a platform without a composition target every preference, including
/// `RequireComposition`, is met by the native window: that surface is the
/// system compositor's, so requiring one there is not a reason to refuse.
pub(crate) fn window_surface_request(
    preference: WindowSurfacePreference,
    wants_transparent_client: bool,
    policy: GpuBackendPolicy,
) -> WindowSurfaceTarget {
    if !PLATFORM_HAS_COMPOSITION_TARGET {
        return WindowSurfaceTarget::NativeWindow;
    }
    match preference {
        WindowSurfacePreference::NativeWindow => WindowSurfaceTarget::NativeWindow,
        WindowSurfacePreference::Composition | WindowSurfacePreference::RequireComposition => {
            WindowSurfaceTarget::Composition
        }
        WindowSurfacePreference::Auto if policy.wants_composition() && wants_transparent_client => {
            WindowSurfaceTarget::Composition
        }
        WindowSurfacePreference::Auto => WindowSurfaceTarget::NativeWindow,
    }
}

/// The target a window gets, given what it asked for and what the process can
/// actually present.
///
/// `required` comes from
/// [`WindowSurfacePreference::RequireComposition`](nana_ui_platform::WindowSurfacePreference::RequireComposition):
/// such a window takes no fallback, and the caller turns the recorded fallback
/// into a refusal to open rather than presenting it the wrong way.
pub(crate) fn resolve_window_surface_target(
    requested: WindowSurfaceTarget,
    required: bool,
    availability: CompositionAvailability,
) -> ResolvedSurfaceTarget {
    if requested.composed() && !availability.available() {
        ResolvedSurfaceTarget::fell_back(
            requested,
            SurfaceTargetFallback::BackendUnavailable,
            required,
        )
    } else {
        ResolvedSurfaceTarget::honoured(requested, required)
    }
}

/// A presentation target after every fallback step, with the request it came
/// from so the outcome stays observable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedSurfaceTarget {
    pub(crate) requested: WindowSurfaceTarget,
    pub(crate) resolved: WindowSurfaceTarget,
    pub(crate) fallback: Option<SurfaceTargetFallback>,
    /// This window would rather not open than present another way.
    pub(crate) required: bool,
}

impl ResolvedSurfaceTarget {
    /// A window presenting through exactly the target it asked for.
    pub(crate) const fn honoured(target: WindowSurfaceTarget, required: bool) -> Self {
        Self {
            requested: target,
            resolved: target,
            fallback: None,
            required,
        }
    }

    /// A window that asked to be composed and is on the plain native path.
    pub(crate) const fn fell_back(
        requested: WindowSurfaceTarget,
        fallback: SurfaceTargetFallback,
        required: bool,
    ) -> Self {
        Self {
            requested,
            resolved: WindowSurfaceTarget::NativeWindow,
            fallback: Some(fallback),
            required,
        }
    }

    /// The fallback this window is not allowed to take, if any. A caller turns
    /// this into a refusal to open.
    pub(crate) const fn forbidden_fallback(self) -> Option<SurfaceTargetFallback> {
        match self.fallback {
            Some(fallback) if self.required => Some(fallback),
            _ => None,
        }
    }
}

/// Demote an effect the surface cannot present.
///
/// Transparent effects clear to a zero alpha, which an `Opaque` surface shows
/// as solid black instead of the desktop behind it. Windows DX12 advertises
/// `Opaque` for every HWND surface, so the request is unsatisfiable there.
///
/// `Gl` is excluded: wgpu-hal hardcodes `Opaque` for every GLES surface and
/// never reads the configured mode back, leaving alpha to the EGL/WGL config,
/// so its alpha mode says nothing about whether the window composites.
fn demote_unpresentable(
    applied: MaterialOutcome,
    alpha_mode: wgpu::CompositeAlphaMode,
    backend: wgpu::Backend,
) -> MaterialOutcome {
    if applied.wants_transparent_surface()
        && alpha_mode == wgpu::CompositeAlphaMode::Opaque
        && backend != wgpu::Backend::Gl
    {
        return MaterialOutcome::solid(MaterialFallback::NativeMaterialUnavailable);
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::Backend::{Dx12, Gl, Vulkan};
    use wgpu::CompositeAlphaMode::{Opaque, PreMultiplied};

    fn settings() -> WindowDescriptor {
        WindowDescriptor::new("presentation")
    }

    #[test]
    fn a_resolved_shadow_is_pending_until_the_host_records_what_it_applied() {
        use nana_ui_platform::{WindowShadow, WindowShadowBackend, WindowShadowFallback};
        for request in [
            WindowShadow::Auto,
            WindowShadow::None,
            WindowShadow::Custom(Default::default()),
        ] {
            let mut resolved = ResolvedWindowPresentation::resolve(
                &settings().shadow(request),
                MaterialEffect::Solid,
                MaterialOutcome::chosen_solid(),
                Opaque,
                Vulkan,
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
                NonClientRenderingStrategy::StripFrameStyles,
            );
            assert_eq!(resolved.shadow().backend, WindowShadowBackend::None);
            assert_eq!(
                resolved.shadow().fallback,
                Some(WindowShadowFallback::Pending)
            );
            resolved.record_shadow(nana_ui_platform::WindowShadowOutcome::disabled());
            assert_eq!(
                resolved.shadow(),
                nana_ui_platform::WindowShadowOutcome::disabled()
            );
        }
    }

    /// The P0 case: a transparent request on a DX12 HWND surface. The surface
    /// negotiated `Opaque`, so the window is solid, and its native chrome has
    /// to be the opaque policy in the very same value — not the transparent
    /// one the request would have asked for.
    #[test]
    fn a_demoted_transparent_request_carries_opaque_chrome() {
        let resolved = ResolvedWindowPresentation::resolve(
            &settings(),
            MaterialEffect::Transparent,
            MaterialOutcome::transparent(),
            Opaque,
            Dx12,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
            NonClientRenderingStrategy::StripFrameStyles,
        );

        assert_eq!(resolved.requested(), MaterialEffect::Transparent);
        assert_eq!(resolved.effective().effect, MaterialEffect::Solid);
        assert_eq!(
            resolved.effective().fallback,
            Some(MaterialFallback::NativeMaterialUnavailable)
        );
        assert_eq!(resolved.chrome(), Some(NativeChromePolicy::OPAQUE));
        assert!(
            resolved.needs_material_reset(),
            "the glass the transparent request extended has to come back off"
        );
    }

    /// A transparent request the surface can present keeps the transparent
    /// policy: DWM must not render a frame the client would show through.
    #[test]
    fn a_presentable_transparent_request_keeps_transparent_chrome() {
        let resolved = ResolvedWindowPresentation::resolve(
            &settings(),
            MaterialEffect::Transparent,
            MaterialOutcome::transparent(),
            PreMultiplied,
            Dx12,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::Composition, false),
            NonClientRenderingStrategy::StripFrameStyles,
        );

        assert_eq!(resolved.effective(), MaterialOutcome::transparent());
        assert_eq!(
            resolved.chrome(),
            Some(NativeChromePolicy::transparent(
                NonClientRenderingStrategy::StripFrameStyles
            ))
        );
        assert!(!resolved.needs_material_reset());
        assert!(resolved.surface_target().composed());
    }

    /// Mica and Acrylic want a transparent surface too, but their background
    /// *is* DWM's non-client rendering, so they keep corners and stroke.
    #[test]
    fn native_materials_keep_the_dwm_frame_they_are_made_of() {
        for effect in [MaterialEffect::Mica, MaterialEffect::Acrylic] {
            let resolved = ResolvedWindowPresentation::resolve(
                &settings(),
                effect,
                MaterialOutcome::native(effect),
                PreMultiplied,
                Dx12,
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
                NonClientRenderingStrategy::StripFrameStyles,
            );
            assert_eq!(resolved.effective().effect, effect);
            assert_eq!(
                resolved.chrome(),
                Some(NativeChromePolicy::OPAQUE),
                "{effect:?} is the DWM frame, so it keeps the round clip"
            );
        }
    }

    /// A GLES surface reports `Opaque` whatever it composites, so its alpha
    /// mode is not evidence that transparency failed.
    #[test]
    fn a_gles_surface_alpha_mode_is_not_a_verdict() {
        let resolved = ResolvedWindowPresentation::resolve(
            &settings(),
            MaterialEffect::Transparent,
            MaterialOutcome::transparent(),
            Opaque,
            Gl,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
            NonClientRenderingStrategy::StripFrameStyles,
        );
        assert_eq!(resolved.effective(), MaterialOutcome::transparent());
        assert_eq!(
            resolved.chrome(),
            Some(NativeChromePolicy::transparent(
                NonClientRenderingStrategy::StripFrameStyles
            ))
        );
    }

    /// A Vulkan HWND surface negotiates premultiplied alpha, so the same
    /// request on the plain path is presentable there.
    #[test]
    fn the_plain_path_is_transparent_where_the_surface_negotiates_it() {
        let resolved = ResolvedWindowPresentation::resolve(
            &settings(),
            MaterialEffect::Transparent,
            MaterialOutcome::transparent(),
            PreMultiplied,
            Vulkan,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
            NonClientRenderingStrategy::StripFrameStyles,
        );
        assert_eq!(
            resolved.chrome(),
            Some(NativeChromePolicy::transparent(
                NonClientRenderingStrategy::StripFrameStyles
            ))
        );
    }

    /// The two ways a transparent client can stop DWM rendering a non-client
    /// area under it, as a policy rather than a fixed branch.
    ///
    /// Stripping the frame styles is measured to work and gives up Aero Snap,
    /// Snap Layouts, Alt+Space and the system window animations. Suppressing
    /// DWM's non-client rendering would keep all of them. Which one a build
    /// uses is a recorded measurement; what the type guarantees is that an
    /// opaque client needs neither, and that the choice reaches the window as
    /// one value rather than two places deciding separately.
    #[test]
    fn a_transparent_client_suppresses_the_non_client_area_either_way() {
        let strip = NativeChromePolicy::transparent(NonClientRenderingStrategy::StripFrameStyles);
        assert!(strip.suppress_non_client);
        assert!(strip.frameless(), "this is the path that loses Aero Snap");
        assert!(!strip.rounded_corners);

        assert!(
            strip.non_client_rendering_enabled(),
            "there is nothing left for DWM to render, so the policy is untouched"
        );

        let suppress =
            NativeChromePolicy::transparent(NonClientRenderingStrategy::SuppressNonClientRendering);
        assert!(suppress.suppress_non_client);
        assert!(
            !suppress.frameless(),
            "keeping the frame styles is the whole point of this strategy"
        );
        assert!(!suppress.rounded_corners);
        assert!(
            !suppress.non_client_rendering_enabled(),
            "this strategy is exactly DWMNCRP_DISABLED"
        );

        // An opaque client's own pixels cover the non-client area, so neither
        // strategy applies to it and it keeps the round clip and the stroke.
        let opaque = NativeChromePolicy::OPAQUE;
        assert!(!opaque.suppress_non_client);
        assert!(!opaque.frameless());
        assert!(opaque.rounded_corners);
        assert!(opaque.non_client_rendering_enabled());

        // The window carries the strategy it was resolved with, so the outcome
        // of the comparison is readable rather than inferred from the build.
        for strategy in [
            NonClientRenderingStrategy::StripFrameStyles,
            NonClientRenderingStrategy::SuppressNonClientRendering,
        ] {
            let resolved = ResolvedWindowPresentation::resolve(
                &settings(),
                MaterialEffect::Transparent,
                MaterialOutcome::transparent(),
                PreMultiplied,
                Dx12,
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::Composition, false),
                strategy,
            );
            assert_eq!(
                resolved.chrome().map(|chrome| chrome.non_client_strategy),
                Some(strategy)
            );
        }
    }

    /// The split the whole item exists for: the GPU backend is a process
    /// concern, the surface target a per-window one.
    ///
    /// One process, one device, three windows with three different answers —
    /// the main window composed because it wants a transparent client, a
    /// Settings window and a popup on the platform's own window surface
    /// because they do not. Nothing about the main window's needs reaches them.
    /// What a composition request resolves to on the platform running the
    /// tests: the compositor visual on Windows, the native window elsewhere.
    fn composed_here() -> WindowSurfaceTarget {
        if PLATFORM_HAS_COMPOSITION_TARGET {
            WindowSurfaceTarget::Composition
        } else {
            WindowSurfaceTarget::NativeWindow
        }
    }

    #[test]
    fn one_composition_capable_process_still_gives_each_window_its_own_target() {
        let policy = GpuBackendPolicy::CompositionCapable;

        // Main: transparent client, default preference.
        assert_eq!(
            window_surface_request(WindowSurfacePreference::Auto, true, policy),
            composed_here()
        );
        // Settings and popups: opaque, default preference.
        assert_eq!(
            window_surface_request(WindowSurfacePreference::Auto, false, policy),
            WindowSurfaceTarget::NativeWindow
        );
        // An explicit request wins either way, which is how #215's shadow
        // companion asks for a compositor visual without the app's other
        // windows changing target.
        assert_eq!(
            window_surface_request(WindowSurfacePreference::Composition, false, policy),
            composed_here()
        );
        assert_eq!(
            window_surface_request(WindowSurfacePreference::NativeWindow, true, policy),
            WindowSurfaceTarget::NativeWindow
        );
    }

    /// A process that never asked for a compositor-capable backend does not
    /// have one, so `Auto` cannot quietly move a transparent window onto a
    /// path the backend was never narrowed for.
    #[test]
    fn a_plain_process_keeps_every_window_on_the_native_surface() {
        let policy = GpuBackendPolicy::Plain;
        assert!(!policy.wants_composition());
        for transparent in [false, true] {
            assert_eq!(
                window_surface_request(WindowSurfacePreference::Auto, transparent, policy),
                WindowSurfaceTarget::NativeWindow
            );
        }
        // An explicit request is still a request; availability decides it.
        let explicit = resolve_window_surface_target(
            window_surface_request(WindowSurfacePreference::Composition, false, policy),
            false,
            CompositionAvailability::Unavailable,
        );
        if PLATFORM_HAS_COMPOSITION_TARGET {
            assert_eq!(
                explicit,
                ResolvedSurfaceTarget::fell_back(
                    WindowSurfaceTarget::Composition,
                    SurfaceTargetFallback::BackendUnavailable,
                    false
                )
            );
        } else {
            assert_eq!(
                explicit,
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false)
            );
        }
    }

    /// A window may ask to fail rather than present another way. That is the
    /// only thing that makes an unavailable compositor a startup failure —
    /// every other window falls back and keeps the application alive.
    #[test]
    fn only_a_window_that_requires_the_compositor_refuses_to_open_without_it() {
        // The composition target itself, as Windows asks for it.
        let required = WindowSurfaceTarget::Composition;

        let refused = resolve_window_surface_target(
            required,
            WindowSurfacePreference::RequireComposition.requires_composition(),
            CompositionAvailability::Unavailable,
        );
        assert_eq!(
            refused.forbidden_fallback(),
            Some(SurfaceTargetFallback::BackendUnavailable)
        );

        // The ordinary composition request takes the fallback instead.
        let tolerant = resolve_window_surface_target(
            WindowSurfaceTarget::Composition,
            WindowSurfacePreference::Composition.requires_composition(),
            CompositionAvailability::Unavailable,
        );
        assert_eq!(tolerant.forbidden_fallback(), None);
        assert_eq!(tolerant.resolved, WindowSurfaceTarget::NativeWindow);

        // And a window that got what it asked for has nothing forbidden.
        assert_eq!(
            resolve_window_surface_target(required, true, CompositionAvailability::Available)
                .forbidden_fallback(),
            None
        );
    }

    /// Off Windows the native window surface is the system compositor's, so a
    /// window that requires composition opens on it rather than refusing —
    /// otherwise such an app could not start on macOS at all.
    #[test]
    fn requiring_composition_is_met_by_the_native_window_where_that_is_composited() {
        let policy = GpuBackendPolicy::CompositionCapable;
        let requested =
            window_surface_request(WindowSurfacePreference::RequireComposition, true, policy);
        let resolved = resolve_window_surface_target(
            requested,
            WindowSurfacePreference::RequireComposition.requires_composition(),
            CompositionAvailability::Unavailable,
        );
        if PLATFORM_HAS_COMPOSITION_TARGET {
            assert_eq!(
                resolved.forbidden_fallback(),
                Some(SurfaceTargetFallback::BackendUnavailable)
            );
        } else {
            assert_eq!(requested, WindowSurfaceTarget::NativeWindow);
            assert_eq!(resolved.forbidden_fallback(), None);
            assert_eq!(resolved.resolved, WindowSurfaceTarget::NativeWindow);
        }
    }

    /// A replacement device can land on another backend. Whether the process
    /// can still compose has to follow it, or a window opened after the switch
    /// would be created without a redirection bitmap for a compositor the new
    /// device cannot reach.
    #[test]
    fn availability_follows_the_device_a_recovery_landed_on() {
        let wanted = GpuBackendPolicy::CompositionCapable;
        assert_eq!(
            CompositionAvailability::for_backend(wanted, Dx12),
            if cfg!(target_os = "windows") {
                CompositionAvailability::Available
            } else {
                CompositionAvailability::Unavailable
            }
        );
        for backend in [Vulkan, Gl] {
            assert_eq!(
                CompositionAvailability::for_backend(wanted, backend),
                CompositionAvailability::Unavailable,
                "{backend:?} has no composition surface target"
            );
        }
        // A process that never asked for it does not acquire it by recovering
        // onto a backend that could have provided it.
        assert_eq!(
            CompositionAvailability::for_backend(GpuBackendPolicy::Plain, Dx12),
            CompositionAvailability::Unavailable
        );
    }

    /// Availability only constrains a window that asked to be composed. A
    /// window on the native surface is never "fallen back".
    #[test]
    fn availability_only_answers_the_windows_that_asked() {
        for availability in [
            CompositionAvailability::Available,
            CompositionAvailability::Unavailable,
        ] {
            assert_eq!(
                resolve_window_surface_target(
                    WindowSurfaceTarget::NativeWindow,
                    false,
                    availability
                ),
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false)
            );
        }
        assert_eq!(
            resolve_window_surface_target(
                WindowSurfaceTarget::Composition,
                false,
                CompositionAvailability::Available
            ),
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::Composition, false)
        );
    }

    /// A window that could not be composed reports the plain path it is really
    /// on, the target it asked for, and why — so a program can see the
    /// difference instead of inferring it from the material.
    #[test]
    fn a_window_that_could_not_be_composed_reports_the_target_it_lost() {
        for reason in [
            SurfaceTargetFallback::BackendUnavailable,
            SurfaceTargetFallback::TargetUnavailable,
        ] {
            let resolved = ResolvedWindowPresentation::resolve(
                &settings(),
                MaterialEffect::Transparent,
                MaterialOutcome::transparent(),
                Opaque,
                Dx12,
                ResolvedSurfaceTarget::fell_back(WindowSurfaceTarget::Composition, reason, false),
                NonClientRenderingStrategy::StripFrameStyles,
            );
            assert_eq!(
                resolved.requested_target(),
                WindowSurfaceTarget::Composition
            );
            assert_eq!(resolved.surface_target(), WindowSurfaceTarget::NativeWindow);
            assert_eq!(resolved.target_fallback(), Some(reason));
            // The plain DX12 path cannot present the transparency either, and
            // that is a separate, separately reported outcome.
            assert_eq!(resolved.effective().effect, MaterialEffect::Solid);
            assert_eq!(resolved.chrome(), Some(NativeChromePolicy::OPAQUE));
            // A later reconcile carries the same target verdict forward.
            assert_eq!(resolved.resolved_target().fallback, Some(reason));
        }
    }

    /// The host owns no part of a system-caption window's frame, whatever it
    /// presents.
    #[test]
    fn a_system_caption_window_has_no_host_chrome_policy() {
        let settings = WindowDescriptor::new("caption").system_caption(true);
        for (applied, alpha) in [
            (MaterialOutcome::transparent(), PreMultiplied),
            (MaterialOutcome::chosen_solid(), Opaque),
        ] {
            let resolved = ResolvedWindowPresentation::resolve(
                &settings,
                applied.effect,
                applied,
                alpha,
                Dx12,
                ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
                NonClientRenderingStrategy::StripFrameStyles,
            );
            assert_eq!(resolved.chrome(), None);
        }
    }
}
