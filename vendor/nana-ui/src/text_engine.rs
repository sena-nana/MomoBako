//! The process-wide `nana-text` engine (Issue #99).
//!
//! One [`SharedTextEngine`] per process: whoever measures a paragraph and
//! whoever rasterizes it must agree on which faces exist and on the ids that
//! name them. The engine owns the font set, the shape cache and the layout
//! cache, so a label shaped for one window is already shaped for the next.
//!
//! This module owns the engine and the raw registrations;
//! [`crate::nana_text`] is the public host API over it (`@font-face`, CSS
//! aliases, the product [`NanaTextShaper`](crate::nana_text::NanaTextShaper)).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nana_text::font::{FaceDescriptor, FontError, FontStyle, FontSystem, GenericFamily, font_blob};
use nana_text::{NativeTextEngine, SharedTextEngine};

/// The bundled UI family, when `bundled-fonts` put it in the database.
#[cfg(feature = "bundled-fonts")]
const BUNDLED_UI_FAMILY: &str = "Noto Sans SC";

static ENGINE: OnceLock<SharedTextEngine> = OnceLock::new();
/// Read once, by whichever call builds [`ENGINE`].
static HERMETIC_REQUESTED: AtomicBool = AtomicBool::new(false);
/// What [`ENGINE`] was actually built with.
static BUILT_HERMETIC: AtomicBool = AtomicBool::new(false);

/// The process-wide `nana-text` engine.
///
/// Built once: a second instance would reparse every bundled face and issue a
/// disjoint set of [`FontId`](nana_text::FontId)s, which is exactly the drift
/// a glyph cache keyed by face id cannot survive.
pub(crate) fn nana_text_engine() -> SharedTextEngine {
    Arc::clone(ENGINE.get_or_init(|| Arc::new(Mutex::new(NativeTextEngine::new(build_fonts())))))
}

/// Build the engine without platform fonts: only the bundled faces, under an
/// empty fallback policy, so every machine measures the same widths. For
/// cross-platform baselines — a glyph the bundled faces lack would otherwise
/// be measured in whatever system font the host happens to fall back to.
///
/// Must run before anything first touches the engine. Returns whether the
/// engine is hermetic; `false` means it was already built with system fonts.
///
/// Needs `bundled-fonts`: without it a hermetic engine holds no face at all,
/// and every measurement would come back empty.
#[cfg(feature = "bundled-fonts")]
pub(crate) fn use_hermetic_fonts() -> bool {
    HERMETIC_REQUESTED.store(true, Ordering::SeqCst);
    // Forcing the build settles a race with a concurrent first caller: once
    // it returns, `BUILT_HERMETIC` records what the engine really holds.
    let _ = nana_text_engine();
    BUILT_HERMETIC.load(Ordering::SeqCst)
}

/// Borrow the shared engine. Poisoning is recovered from: a panic elsewhere
/// leaves no half-applied state, because every cache insert is one operation.
pub(crate) fn lock_engine(
    shared: &SharedTextEngine,
) -> std::sync::MutexGuard<'_, NativeTextEngine> {
    nana_text::lock_text_engine(shared)
}

/// The face-set generation the engine is currently on.
#[cfg(feature = "gpu")]
pub(crate) fn engine_font_generation() -> u64 {
    use nana_text::TextEngine as _;
    let engine = nana_text_engine();
    let engine = lock_engine(&engine);
    engine.font_generation().get()
}

fn build_fonts() -> FontSystem {
    let hermetic = HERMETIC_REQUESTED.load(Ordering::SeqCst);
    BUILT_HERMETIC.store(hermetic, Ordering::SeqCst);
    #[allow(unused_mut)]
    let mut fonts = if hermetic {
        FontSystem::hermetic()
    } else {
        FontSystem::with_system_fonts()
    };
    #[cfg(target_os = "android")]
    {
        // The platform scan covers no directory on Android. Registered file
        // by file so a single unreadable face does not lose the directory.
        if !hermetic && let Ok(entries) = std::fs::read_dir("/system/fonts") {
            for entry in entries.flatten() {
                let _ = fonts.register_file(entry.path(), &FaceDescriptor::default());
            }
        }
    }
    #[cfg(feature = "bundled-fonts")]
    {
        for source in crate::ui_font_sources() {
            let _ = fonts.register_bytes(font_blob(source), &FaceDescriptor::default());
        }
        let mut policy = fonts.policy().clone();
        policy.set_generic(GenericFamily::SansSerif, [BUNDLED_UI_FAMILY]);
        policy.set_generic(GenericFamily::SystemUi, [BUNDLED_UI_FAMILY]);
        fonts.set_policy(policy);
    }
    fonts
}

/// What [`crate::nana_text::register_host_font_face`] does to the one font
/// set: `@font-face` bytes under a declared CSS family and weight range.
///
/// Returns the number of faces the bytes contained, or 0 when they are not a
/// font. The weight range is registered as one face —
/// [`FaceDescriptor::weight`] is a range, not one alias per 100-weight step.
pub(crate) fn register_face_bytes(
    family: &str,
    data: Vec<u8>,
    weight: Option<u16>,
    weight_end: Option<u16>,
    style: Option<FontStyle>,
) -> Result<usize, FontError> {
    let descriptor = FaceDescriptor {
        family: Some(Arc::from(family)),
        weight: weight.map(|start| {
            let end = weight_end.unwrap_or(start);
            (f32::from(start.min(end)), f32::from(start.max(end)))
        }),
        stretch: None,
        style,
    };
    let engine = nana_text_engine();
    let mut engine = lock_engine(&engine);
    engine
        .fonts_mut()
        .register_bytes(font_blob(data), &descriptor)
        .map(|registration| registration.faces.len())
}

/// Mirror of [`crate::nana_text::register_host_font_bytes`]: bytes under their
/// own family names.
pub(crate) fn register_bytes(data: Vec<u8>) -> Result<usize, FontError> {
    let engine = nana_text_engine();
    let mut engine = lock_engine(&engine);
    engine
        .fonts_mut()
        .register_bytes(font_blob(data), &FaceDescriptor::default())
        .map(|registration| registration.faces.len())
}

/// Mirror of [`crate::nana_text::register_host_font_file`].
pub(crate) fn register_file(path: &std::path::Path) -> Result<usize, FontError> {
    let engine = nana_text_engine();
    let mut engine = lock_engine(&engine);
    engine
        .fonts_mut()
        .register_file(path, &FaceDescriptor::default())
        .map(|registration| registration.faces.len())
}

/// Mirror of [`crate::nana_text::alias_host_font_face_local`]: bind a declared
/// CSS family to faces already in the database.
///
/// The font system matches by family name, so an alias is a re-registration of
/// the same bytes under the declared family. Faces whose bytes cannot be read
/// back (a lazily-scanned system file that has since gone) are skipped.
pub(crate) fn alias_local_family(
    css_family: &str,
    local_family: &str,
    weight: Option<u16>,
    weight_end: Option<u16>,
) -> usize {
    let engine = nana_text_engine();
    let mut engine = lock_engine(&engine);
    let fonts = engine.fonts_mut();
    let local = local_family.to_ascii_lowercase();
    let matched: Vec<_> = fonts
        .faces()
        .into_iter()
        .filter_map(|id| fonts.describe(id).map(|face| (id, face)))
        .filter(|(_, face)| {
            face.families
                .iter()
                .any(|name| name.to_ascii_lowercase() == local)
                || face.post_script_name.to_ascii_lowercase() == local
        })
        .map(|(id, _)| id)
        .collect();
    if matched.is_empty() {
        return 0;
    }
    let descriptor = FaceDescriptor {
        family: Some(Arc::from(css_family)),
        weight: weight.map(|start| {
            let end = weight_end.unwrap_or(start);
            (f32::from(start.min(end)), f32::from(start.max(end)))
        }),
        stretch: None,
        style: None,
    };
    let mut aliased = 0usize;
    for id in matched {
        let Some(data) = fonts.face_data(id) else {
            continue;
        };
        let bytes = data.bytes().to_vec();
        if fonts.register_bytes(font_blob(bytes), &descriptor).is_ok() {
            aliased += 1;
        }
    }
    aliased
}

/// Mirror of [`crate::nana_text::set_sans_serif_family`].
pub(crate) fn set_sans_serif_family(name: &str) {
    let engine = nana_text_engine();
    let mut engine = lock_engine(&engine);
    let mut policy = engine.fonts().policy().clone();
    policy.set_generic(GenericFamily::SansSerif, [name]);
    policy.set_generic(GenericFamily::SystemUi, [name]);
    engine.fonts_mut().set_policy(policy);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_is_one_instance_for_the_whole_process() {
        let first = nana_text_engine();
        let second = nana_text_engine();
        assert!(
            Arc::ptr_eq(&first, &second),
            "two engines would issue two disjoint FontId spaces"
        );
    }

    #[cfg(feature = "bundled-fonts")]
    #[test]
    fn the_bundled_ui_family_is_registered_and_is_what_sans_serif_resolves_to() {
        let engine = nana_text_engine();
        let engine = lock_engine(&engine);
        let fonts = engine.fonts();
        let has_bundled = fonts
            .faces()
            .into_iter()
            .filter_map(|id| fonts.describe(id))
            .any(|face| {
                face.families
                    .iter()
                    .any(|name| name.as_ref() == BUNDLED_UI_FAMILY)
            });
        assert!(has_bundled, "bundled UI faces must be in the database");
        assert_eq!(
            fonts
                .policy()
                .generic(GenericFamily::SansSerif)
                .first()
                .map(|name| name.as_ref()),
            Some(BUNDLED_UI_FAMILY)
        );
    }
}
