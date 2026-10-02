//! Desktop window shadows: what a window's shadow should be, the body it
//! follows, and the outcome the platform actually applied (#215).
//!
//! One authority decides: [`intent`] from the request and the effective
//! material, applied by `nana_window::shadow::WindowShadowState`, whose answer
//! is recorded on the window's presentation as [`outcome`]. Nothing reports a
//! shadow the platform did not show.

use nana_ui_platform::{
    WindowDescriptor, WindowShadow, WindowShadowBackend, WindowShadowFallback, WindowShadowOutcome,
    WindowShadowSource, WindowShadowStyle, WindowVisualShape,
};
use nana_ui_scene::{ScenePrimitiveKind, UiScene};
use nana_window::shadow::{ShadowApplied, ShadowPlan, ShadowShape, ShadowStyle};

/// What a window's shadow should be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ShadowIntent {
    Disable,
    /// The platform's own shadow. When it cannot draw one (a frameless
    /// Windows window has no DWM frame) the default companion stands in.
    Native,
    /// A companion following the window's visual shape.
    Companion(WindowShadowStyle),
    Unavailable(WindowShadowFallback),
}

/// The request resolved against what the window presents.
///
/// `Auto` on an opaque or material window is the platform's own shadow; on a
/// transparent window the platform would outline whatever the client paints
/// (AppKit) or draw nothing (DWM), so it is a companion that follows the
/// visible card. A custom style is always a companion: no platform shadow
/// takes one.
pub(crate) fn intent(settings: &WindowDescriptor, transparent: bool) -> ShadowIntent {
    match settings.shadow {
        WindowShadow::None => ShadowIntent::Disable,
        WindowShadow::Auto if transparent => ShadowIntent::Companion(WindowShadowStyle::default()),
        WindowShadow::Auto => ShadowIntent::Native,
        WindowShadow::Custom(style) if !style.is_valid() => {
            ShadowIntent::Unavailable(WindowShadowFallback::InvalidStyle)
        }
        WindowShadow::Custom(style) if style.source == WindowShadowSource::ContentAlpha => {
            ShadowIntent::Unavailable(WindowShadowFallback::ContentAlphaUnavailable)
        }
        WindowShadow::Custom(style) => ShadowIntent::Companion(style),
    }
}

/// Before the host has applied anything: the reason when the request cannot
/// be met at all, `Pending` otherwise.
pub(crate) fn pending(intent: ShadowIntent) -> WindowShadowOutcome {
    match intent {
        ShadowIntent::Unavailable(reason) => WindowShadowOutcome::unavailable(reason),
        _ => WindowShadowOutcome::unavailable(WindowShadowFallback::Pending),
    }
}

pub(crate) fn style(style: WindowShadowStyle) -> ShadowStyle {
    ShadowStyle {
        color: style.color,
        offset: style.offset,
        blur: style.blur,
        spread: style.spread,
    }
}

pub(crate) fn shape(shape: WindowVisualShape) -> ShadowShape {
    ShadowShape {
        rect: shape.rect,
        radii: shape.radii,
    }
}

/// The plan for `intent` around `body`.
pub(crate) fn plan(intent: ShadowIntent, body: WindowVisualShape) -> Option<ShadowPlan> {
    match intent {
        ShadowIntent::Disable => Some(ShadowPlan::Disable),
        ShadowIntent::Native => Some(ShadowPlan::Native),
        ShadowIntent::Companion(request) => Some(ShadowPlan::Companion {
            style: style(request),
            shape: shape(body),
        }),
        ShadowIntent::Unavailable(_) => None,
    }
}

/// The outcome to report for what the platform applied.
pub(crate) fn outcome(intent: ShadowIntent, applied: ShadowApplied) -> WindowShadowOutcome {
    if let ShadowIntent::Unavailable(reason) = intent {
        return WindowShadowOutcome::unavailable(reason);
    }
    match applied {
        ShadowApplied::Disabled => WindowShadowOutcome::disabled(),
        ShadowApplied::Native => WindowShadowOutcome {
            backend: WindowShadowBackend::Native,
            fallback: None,
        },
        ShadowApplied::Companion => WindowShadowOutcome {
            backend: WindowShadowBackend::Companion,
            fallback: None,
        },
        ShadowApplied::NativeNotRemovable => WindowShadowOutcome {
            backend: WindowShadowBackend::Native,
            fallback: Some(WindowShadowFallback::DisableUnsupported),
        },
        ShadowApplied::Unsupported => WindowShadowOutcome::unavailable(
            if cfg!(any(target_os = "macos", target_os = "windows")) {
                WindowShadowFallback::Unsupported
            } else {
                // Linux and the rest: the compositor or server-side
                // decorations decide, and nothing here can observe it.
                WindowShadowFallback::CompositorManaged
            },
        ),
        ShadowApplied::Failed => {
            WindowShadowOutcome::unavailable(WindowShadowFallback::BackendUnavailable)
        }
    }
}

/// How many primitives, in paint order, to look through for the body. The
/// root card is painted first; a window whose first primitives are not it
/// has no single body to follow.
const BODY_SEARCH: usize = 16;

/// The visible body of a window: the first large, filled background quad the
/// scene paints — a transparent window's root card — with its corner radii,
/// in logical client coordinates. Falls back to the whole client area.
///
/// Called only when the scene's projection or the window size changed; the
/// caller keeps the answer otherwise.
pub(crate) fn derive_visual_shape(scene: &UiScene, logical: [f32; 2]) -> WindowVisualShape {
    let client = WindowVisualShape::client(logical[0].max(1.0), logical[1].max(1.0));
    let area = logical[0] * logical[1];
    for primitive in scene.primitives().take(BODY_SEARCH) {
        let ScenePrimitiveKind::Quad {
            background: Some(background),
            corner_radius,
            ..
        } = &primitive.kind
        else {
            continue;
        };
        if background[3] <= 0.0 {
            continue;
        }
        // Only a translation places the body; a rotated or scaled card has no
        // rounded rectangle to follow.
        let [a, b, c, d, e, f] = primitive.transform.0;
        if (a, b, c, d) != (1.0, 0.0, 0.0, 1.0) || primitive.transform.1 != [0.0, 0.0] {
            continue;
        }
        let bounds = primitive.bounds;
        if bounds.width * bounds.height < area * 0.25 {
            continue;
        }
        let shape = WindowVisualShape {
            rect: [bounds.x + e, bounds.y + f, bounds.width, bounds.height],
            radii: *corner_radius,
        };
        if shape.is_valid() {
            return shape;
        }
    }
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(shadow: WindowShadow) -> WindowDescriptor {
        WindowDescriptor::new("shadow").shadow(shadow)
    }

    #[test]
    fn auto_is_native_when_opaque_and_a_companion_when_transparent() {
        assert_eq!(
            intent(&settings(WindowShadow::Auto), false),
            ShadowIntent::Native
        );
        assert_eq!(
            intent(&settings(WindowShadow::Auto), true),
            ShadowIntent::Companion(WindowShadowStyle::default())
        );
        assert_eq!(
            intent(&settings(WindowShadow::None), true),
            ShadowIntent::Disable
        );
    }

    #[test]
    fn custom_styles_are_companions_and_unsupported_sources_say_why() {
        let custom = WindowShadowStyle {
            blur: 24.0,
            ..Default::default()
        };
        assert_eq!(
            intent(&settings(WindowShadow::Custom(custom)), false),
            ShadowIntent::Companion(custom)
        );
        let alpha = WindowShadowStyle {
            source: WindowShadowSource::ContentAlpha,
            ..Default::default()
        };
        assert_eq!(
            pending(intent(&settings(WindowShadow::Custom(alpha)), true)),
            WindowShadowOutcome::unavailable(WindowShadowFallback::ContentAlphaUnavailable)
        );
        let invalid = WindowShadowStyle {
            blur: -1.0,
            ..Default::default()
        };
        assert_eq!(
            pending(intent(&settings(WindowShadow::Custom(invalid)), true)),
            WindowShadowOutcome::unavailable(WindowShadowFallback::InvalidStyle)
        );
    }

    #[test]
    fn outcomes_report_only_what_was_applied() {
        assert_eq!(
            pending(ShadowIntent::Native).fallback,
            Some(WindowShadowFallback::Pending)
        );
        assert_eq!(
            outcome(ShadowIntent::Disable, ShadowApplied::NativeNotRemovable),
            WindowShadowOutcome {
                backend: WindowShadowBackend::Native,
                fallback: Some(WindowShadowFallback::DisableUnsupported),
            }
        );
        assert_eq!(
            outcome(ShadowIntent::Native, ShadowApplied::Failed),
            WindowShadowOutcome::unavailable(WindowShadowFallback::BackendUnavailable)
        );
    }

    fn card_scene(card: [f32; 2], inset: f32) -> UiScene {
        use nana_ui_core::LengthSpec;
        use nana_ui_runtime::{DocumentId, LayoutViewport, MeasureTextShaper, Stack};
        let document_id = DocumentId::new(1).unwrap();
        let mut document = nana_ui_scene::RuntimeDocument::new(document_id);
        use nana_ui_runtime::view::widget;
        document
            .context_mut()
            .mount_view_root(document_id, || {
                let root = Stack::column(0.0).with_layout(|layout| {
                    layout.padding = Some(LengthSpec::Px(inset));
                });
                let card = Stack::column(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(card[0]));
                    layout.height = Some(LengthSpec::Px(card[1]));
                    layout.background = Some([1.0, 1.0, 1.0, 1.0]);
                    layout.border_radius = Some(12.0);
                });
                widget(root).children(widget(card))
            })
            .unwrap();
        document
            .flush(LayoutViewport::new(400.0, 300.0), &mut MeasureTextShaper)
            .unwrap();
        document.scene().clone()
    }

    #[test]
    fn the_body_is_the_inset_rounded_card_the_window_paints() {
        let scene = card_scene([360.0, 260.0], 20.0);
        assert_eq!(
            derive_visual_shape(&scene, [400.0, 300.0]),
            WindowVisualShape {
                rect: [20.0, 20.0, 360.0, 260.0],
                radii: [12.0; 4],
            }
        );
        // A small badge is not the window's body.
        let badge = card_scene([40.0, 20.0], 20.0);
        assert_eq!(
            derive_visual_shape(&badge, [400.0, 300.0]),
            WindowVisualShape::client(400.0, 300.0)
        );
    }

    #[test]
    fn an_empty_scene_falls_back_to_the_client_area() {
        let scene = UiScene::new();
        assert_eq!(
            derive_visual_shape(&scene, [300.0, 200.0]),
            WindowVisualShape::client(300.0, 200.0)
        );
    }
}
