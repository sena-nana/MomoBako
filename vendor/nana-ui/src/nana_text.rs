//! The host text API: `@font-face` ingest and the product [`TextShaper`].
//!
//! Since #99 there is exactly one measurement authority in the process, the
//! `nana-text` engine of [`crate::text_engine`], and it is the same one
//! `NanaRenderer::text` resolves its glyphs from. Measuring with one engine and
//! drawing with another is the split this Epic exists to remove, so this module
//! holds no shaping of its own — it registers faces and forwards.

use nana_ui_runtime::{
    ComputedStyle, GlyphCache, LayoutBox, NanaTextEngineShaper, StableNodeId, TextContent,
    TextMetrics, TextShapeConstraints, TextShaper,
};
use std::path::Path;

/// Why a host font source could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFontError {
    Empty,
    Unrecognized,
    Io(String),
}

impl std::fmt::Display for HostFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "host font bytes were empty"),
            Self::Unrecognized => write!(f, "host font bytes were not a recognized font face"),
            Self::Io(err) => write!(f, "host font file: {err}"),
        }
    }
}

impl std::error::Error for HostFontError {}

impl From<nana_text::font::FontError> for HostFontError {
    fn from(error: nana_text::font::FontError) -> Self {
        match error {
            nana_text::font::FontError::Empty => Self::Empty,
            nana_text::font::FontError::Io(message) => Self::Io(message),
            nana_text::font::FontError::Unrecognized
            | nana_text::font::FontError::UnknownSource => Self::Unrecognized,
        }
    }
}

/// CSS `@font-face` `font-style` mapped onto a loaded face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFontStyle {
    Normal,
    Italic,
    Oblique,
}

impl HostFontStyle {
    fn to_font_style(self) -> nana_text::font::FontStyle {
        match self {
            Self::Normal => nana_text::font::FontStyle::Normal,
            Self::Italic => nana_text::font::FontStyle::Italic,
            Self::Oblique => nana_text::font::FontStyle::Oblique,
        }
    }
}

/// Largest `@font-face` payload accepted, matching the `url()` ingest cap.
const FONT_FACE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Register a CSS `@font-face` source into the process-wide font set.
///
/// The declared `family` **replaces** the face's own names, as `@font-face`
/// does, and `weight` / `weight_end` are the CSS range (`font-weight: 200
/// 700`) — one inclusive range, not one alias per 100-step: since #99 the font
/// layer matches ranges natively.
///
/// Returns the number of faces the source contained (0 on failure).
pub fn register_host_font_face(
    family: &str,
    data: Vec<u8>,
    weight: Option<u16>,
    weight_end: Option<u16>,
) -> usize {
    register_host_font_face_styled(family, data, weight, weight_end, None)
}

/// [`register_host_font_face`] with an explicit `@font-face` `font-style`.
pub fn register_host_font_face_styled(
    family: &str,
    data: Vec<u8>,
    weight: Option<u16>,
    weight_end: Option<u16>,
    style: Option<HostFontStyle>,
) -> usize {
    let family = family.trim();
    if family.is_empty() || data.is_empty() || data.len() as u64 > FONT_FACE_MAX_BYTES {
        return 0;
    }
    crate::text_engine::register_face_bytes(
        family,
        data,
        weight,
        weight_end,
        style.map(HostFontStyle::to_font_style),
    )
    .unwrap_or(0)
}

/// Bind `@font-face` `local("Family")` to faces already registered.
///
/// Matches family names and PostScript names, ASCII case-insensitively. Does
/// not load bytes or follow `url()`. Returns the number of faces bound.
pub fn alias_host_font_face_local(
    css_family: &str,
    local_family: &str,
    weight: Option<u16>,
    weight_end: Option<u16>,
) -> usize {
    let css_family = css_family.trim();
    let local_family = local_family.trim();
    if css_family.is_empty() || local_family.is_empty() {
        return 0;
    }
    crate::text_engine::alias_local_family(css_family, local_family, weight, weight_end)
}

/// Load font bytes under the face's own family names.
pub fn register_host_font_bytes(bytes: impl Into<Vec<u8>>) -> Result<usize, HostFontError> {
    let bytes = bytes.into();
    if bytes.is_empty() {
        return Err(HostFontError::Empty);
    }
    crate::text_engine::register_bytes(bytes).map_err(HostFontError::from)
}

/// Load a font file (or collection) from `path`.
pub fn register_host_font_file(path: impl AsRef<Path>) -> Result<usize, HostFontError> {
    crate::text_engine::register_file(path.as_ref()).map_err(HostFontError::from)
}

/// Set the generic `sans-serif` family. `bundled-fonts` already sets
/// `Noto Sans SC`.
pub fn set_sans_serif_family(name: impl AsRef<str>) {
    crate::text_engine::set_sans_serif_family(name.as_ref());
}

/// Measure and draw with the bundled faces only, never the platform's.
///
/// For baselines that must agree across machines. Call before any text is
/// shaped; returns `false` when the engine was already built with system
/// fonts, in which case nothing changed.
///
/// Only with `bundled-fonts`: those faces are the whole font set a hermetic
/// engine has, so without them there would be nothing left to measure with.
#[cfg(feature = "bundled-fonts")]
pub fn use_hermetic_fonts() -> bool {
    crate::text_engine::use_hermetic_fonts()
}

/// Family names of the faces the engine actually used to shape `text` when
/// asked for `family`.
///
/// The diagnostic behind the `@font-face` tests: it answers "did the alias
/// win, or did fallback quietly pick something else".
pub fn shaped_face_families(family: &str, text: &str) -> Vec<String> {
    let source = nana_text::TextSource::new(text);
    let style = nana_text::TextStyle {
        font_family: Some(std::sync::Arc::from(family)),
        ..nana_text::TextStyle::default()
    };
    let constraints = nana_text::TextConstraints::default();
    let engine = crate::text_engine::nana_text_engine();
    let mut engine = crate::text_engine::lock_engine(&engine);
    let mut counters = nana_text::TextWorkCounters::default();
    let layout = nana_text::TextEngine::layout(
        &mut *engine,
        nana_text::TextKind::Label,
        &source,
        &style,
        &constraints,
        &mut counters,
    );
    let mut names: Vec<String> = layout
        .runs
        .iter()
        .filter_map(|run| engine.fonts().describe(run.font))
        .flat_map(|face| {
            face.families
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Product text shaper for Runtime flush on the Nana WGPU host path.
///
/// Since #99 this is the `nana-text` engine and nothing else: the name is what
/// hosts construct, and the measurement authority behind it is the same engine
/// the painter resolves its glyphs from.
#[derive(Debug, Clone)]
pub struct NanaTextShaper {
    engine: NanaTextEngineShaper,
}

impl Default for NanaTextShaper {
    fn default() -> Self {
        Self {
            engine: NanaTextEngineShaper::new(crate::text_engine::nana_text_engine()),
        }
    }
}

// Every `TextShaper` method below forwards to the engine shaper. A wrapper
// rather than a re-export: `NanaTextShaper` is the name hosts, examples and
// tests construct, and #99 changes what measures behind it without changing
// what they write.
impl TextShaper for NanaTextShaper {
    fn font_generation(&self) -> u64 {
        self.engine.font_generation()
    }

    fn retains_measurement(&self, id: StableNodeId) -> bool {
        self.engine.retains_measurement(id)
    }

    fn text_engine(&self) -> Option<nana_text::SharedTextEngine> {
        self.engine.text_engine()
    }

    fn take_text_work(&mut self) -> nana_text::TextWorkCounters {
        self.engine.take_text_work()
    }

    fn shape(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> TextMetrics {
        self.engine.shape(id, text, style, constraints)
    }

    fn shape_cached(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
        glyphs: &mut GlyphCache,
    ) -> TextMetrics {
        self.engine
            .shape_cached(id, text, style, constraints, glyphs)
    }

    fn horizontal_offset(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        style: &ComputedStyle,
    ) -> f32 {
        self.engine.horizontal_offset(id, text, byte_offset, style)
    }

    fn text_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.engine
            .text_position(id, text, byte_offset, style, constraints)
    }

    fn text_caret_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        affinity: nana_text::Affinity,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.engine
            .text_caret_position(id, text, byte_offset, affinity, style, constraints)
    }

    fn text_highlights(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        selection: (usize, usize),
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> Vec<LayoutBox> {
        self.engine
            .text_highlights(id, text, selection, style, constraints)
    }

    fn text_hit_at_point(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        x: f32,
        y: f32,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> Option<nana_ui_runtime::TextHit> {
        self.engine
            .text_hit_at_point(id, text, x, y, style, constraints)
    }

    fn text_caret_visual_step(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        affinity: nana_text::Affinity,
        rightwards: bool,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> Option<nana_ui_runtime::TextHit> {
        self.engine.text_caret_visual_step(
            id,
            text,
            byte_offset,
            affinity,
            rightwards,
            style,
            constraints,
        )
    }

    fn with_text_probes<R>(
        &mut self,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
        consume: impl FnOnce(&mut dyn TextShaper) -> R,
    ) -> R {
        self.engine
            .with_text_probes(text, style, constraints, consume)
    }
}

#[cfg(test)]
mod tests {
    // Font registration bumps the process-wide generation, which every other
    // test's caches are keyed against. Serialize them.
    static FONT_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use super::*;
    use nana_ui_core::{FontVariationSetting, LineHeightSpec};
    use nana_ui_runtime::TextShaping;

    fn node() -> StableNodeId {
        StableNodeId::new(1).unwrap()
    }

    fn assert_positive_finite(metrics: TextMetrics) {
        assert!(metrics.width.is_finite() && metrics.width > 0.0);
        assert!(metrics.height.is_finite() && metrics.height > 0.0);
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn register_host_font_face_aliases_css_family() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let data = include_bytes!("../assets/fonts/NotoSansSC-Regular.ttf");
        let added = register_host_font_face("NanaCssFace", data.to_vec(), Some(400), None);
        assert!(added > 0, "bundled Regular face must load");
        let used = shaped_face_families("NanaCssFace", "H");
        assert!(
            used.iter().any(|name| name == "NanaCssFace"),
            "the declared family must be what shapes, used={used:?}"
        );
    }

    #[test]
    fn register_host_font_face_rejects_garbage() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            register_host_font_face("Nope", b"not-a-font".to_vec(), Some(400), None),
            0
        );
        assert_eq!(
            register_host_font_face("", b"whatever".to_vec(), None, None),
            0
        );
    }

    /// A CSS weight range is one registration, not one alias per 100-step, so
    /// what has to hold is that asking anywhere inside it lands on the face.
    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn a_weight_range_matches_every_weight_in_it() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let data = include_bytes!("../assets/fonts/NotoSansSC-Regular.ttf");
        let added = register_host_font_face("NanaVfRangeFace", data.to_vec(), Some(200), Some(700));
        assert!(added > 0, "bundled Regular face must load");
        for weight in [200u16, 400, 700] {
            let mut shaper = NanaTextShaper::default();
            let metrics = shaper.shape(
                node(),
                &TextContent { value: "H".into() },
                &ComputedStyle {
                    font_family: Some("NanaVfRangeFace".into()),
                    font_weight: Some(weight),
                    ..ComputedStyle::default()
                },
                TextShapeConstraints::default(),
            );
            assert_positive_finite(metrics);
        }
    }

    #[test]
    fn alias_host_font_face_local_unknown_family_is_zero() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            alias_host_font_face_local("NopeLocal", "DefinitelyNotANanaFont_xyz", None, None),
            0
        );
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn alias_host_font_face_local_binds_bundled_noto() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let added = alias_host_font_face_local("NanaBundledLocal", "Noto Sans SC", Some(400), None);
        assert!(added > 0, "bundled Noto Sans SC must satisfy local()");
        let used = shaped_face_families("NanaBundledLocal", "H");
        assert!(
            used.iter().any(|name| name == "NanaBundledLocal"),
            "local() alias must be what shapes, used={used:?}"
        );
    }

    /// A character measured before is measured the same way again: no cache
    /// in front of the engine may answer with less than the layout does (a
    /// missing ascent moves a one-character label's baseline by 0.2em).
    #[test]
    fn a_measurement_does_not_depend_on_what_was_measured_before() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut shaper = NanaTextShaper::default();
        let mut glyphs = GlyphCache::default();
        let style = ComputedStyle {
            font_size: 14.0,
            ..ComputedStyle::default()
        };
        let id = StableNodeId::new(1).unwrap();
        let text = TextContent { value: "A".into() };
        let constraints = TextShapeConstraints::default();
        let cold = shaper.shape_cached(id, &text, &style, constraints, &mut glyphs);
        let warm = shaper.shape_cached(id, &text, &style, constraints, &mut glyphs);
        assert!(cold.ascent.is_some());
        assert_eq!(warm, cold);
    }

    #[test]
    fn every_shaper_measures_through_one_engine() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let first = NanaTextShaper::default();
        let second = NanaTextShaper::default();
        let painter_side = crate::text_engine::nana_text_engine();
        let engine_of = |shaper: &NanaTextShaper| shaper.text_engine().expect("engine-backed");
        assert!(std::sync::Arc::ptr_eq(
            &engine_of(&first),
            &engine_of(&second)
        ));
        assert!(std::sync::Arc::ptr_eq(&engine_of(&first), &painter_side));

        let mut shaper = second;
        let metrics = shaper.shape(
            node(),
            &TextContent {
                value: "shared".into(),
            },
            &ComputedStyle::default(),
            TextShapeConstraints::default(),
        );
        assert_positive_finite(metrics);
    }

    #[test]
    fn shapes_ascii_within_a_finite_max_width() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let metrics = NanaTextShaper::default().shape(
            node(),
            &TextContent {
                value: "Hello, Nana".into(),
            },
            &ComputedStyle::default(),
            TextShapeConstraints {
                max_width: Some(240.0),
                wrap: true,
                shaping: TextShaping::Advanced,
                ..TextShapeConstraints::default()
            },
        );
        assert_positive_finite(metrics);
    }

    #[test]
    fn shapes_cjk_weekday_with_nonzero_width() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let metrics = NanaTextShaper::default().shape(
            node(),
            &TextContent {
                value: "周一".into(),
            },
            &ComputedStyle::default(),
            TextShapeConstraints::default(),
        );
        assert_positive_finite(metrics);
    }

    #[test]
    fn invalid_byte_offsets_return_zero_and_empty_highlights() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut shaper = NanaTextShaper::default();
        let text = TextContent {
            value: "周一👩‍💻".into(),
        };
        let style = ComputedStyle::default();
        let constraints = TextShapeConstraints::default();
        let mid_char = 1;
        let past_end = text.value.len() + 4;

        assert_eq!(
            shaper.horizontal_offset(node(), &text, mid_char, &style),
            0.0
        );
        assert_eq!(
            shaper.horizontal_offset(node(), &text, past_end, &style),
            0.0
        );
        assert_eq!(
            shaper.text_position(node(), &text, past_end, &style, constraints),
            (0.0, 0.0, 0.0)
        );
        assert!(
            shaper
                .text_highlights(node(), &text, (0, past_end), &style, constraints)
                .is_empty()
        );
        assert!(
            shaper
                .text_highlights(node(), &text, (text.value.len(), 0), &style, constraints)
                .is_empty()
        );
    }

    #[test]
    fn highlight_rects_are_ordered_and_finite() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut shaper = NanaTextShaper::default();
        let style = ComputedStyle {
            font_size: 16.0,
            line_height: Some(LineHeightSpec::Absolute(20.0)),
            ..ComputedStyle::default()
        };
        let two_cjk = shaper.shape(
            node(),
            &TextContent {
                value: "甲乙".into(),
            },
            &style,
            TextShapeConstraints {
                shaping: TextShaping::Advanced,
                ..TextShapeConstraints::default()
            },
        );
        let wrapped = TextShapeConstraints {
            max_width: Some(two_cjk.width + 1.0),
            wrap: true,
            shaping: TextShaping::Advanced,
            ..TextShapeConstraints::default()
        };
        let text = TextContent {
            value: "甲乙👩‍💻丙丁戊".into(),
        };
        let highlights = shaper.text_highlights(
            node(),
            &text,
            ("甲".len(), "甲乙👩‍💻丙丁".len()),
            &style,
            wrapped,
        );

        assert!(highlights.len() >= 2, "a wrapped selection spans lines");
        assert!(highlights.windows(2).all(|lines| lines[0].y < lines[1].y));
        assert!(highlights.iter().all(|line| {
            line.width.is_finite()
                && line.height.is_finite()
                && line.x.is_finite()
                && line.y.is_finite()
                && line.width > 0.0
                && line.height > 0.0
        }));

        let unknown = shaper.shape(
            node(),
            &TextContent {
                value: "Hello".into(),
            },
            &ComputedStyle {
                font_family: Some("DefinitelyNotARealFamily".into()),
                ..ComputedStyle::default()
            },
            TextShapeConstraints::default(),
        );
        assert_positive_finite(unknown);
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn letter_spacing_widens_shaped_metrics() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let text = TextContent {
            value: "标题文字".into(),
        };
        let base = ComputedStyle {
            font_size: 16.0,
            font_family: Some("Noto Sans SC".into()),
            ..ComputedStyle::default()
        };
        let tight =
            NanaTextShaper::default().shape(node(), &text, &base, TextShapeConstraints::default());
        let tracked = NanaTextShaper::default().shape(
            node(),
            &text,
            &ComputedStyle {
                letter_spacing: 0.5,
                ..base
            },
            TextShapeConstraints::default(),
        );
        assert!(tight.width.is_finite() && tight.width > 0.0);
        assert!(
            tracked.width > tight.width,
            "0.5px tracking must be visible in layout width, tight={} tracked={}",
            tight.width,
            tracked.width
        );
    }

    /// #59: vertical text is measured as a column — a line box across, its
    /// vertical advances down — and an editor's offsets stay across a line.
    #[test]
    fn vertical_text_measures_as_a_column() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut shaper = NanaTextShaper::default();
        let horizontal = ComputedStyle {
            font_size: 16.0,
            ..ComputedStyle::default()
        };
        let vertical = ComputedStyle {
            writing_mode: nana_ui_core::WritingModeSpec::VerticalRl,
            ..horizontal.clone()
        };
        let constraints = TextShapeConstraints {
            shaping: TextShaping::Advanced,
            ..TextShapeConstraints::default()
        };
        let column = shaper.shape(
            node(),
            &TextContent {
                value: "やや".into(),
            },
            &vertical,
            constraints,
        );
        assert!(
            column.height > column.width,
            "two kana stand in a column: {column:?}"
        );
        assert_eq!(
            column.ascent, None,
            "a column hangs from a central baseline"
        );
        let single = shaper.shape(
            node(),
            &TextContent {
                value: "や".into()
            },
            &vertical,
            constraints,
        );
        assert!(
            (single.width - 16.0 * 1.2).abs() < 0.01,
            "one column is one line box wide: {single:?}"
        );

        // An editor's offsets are across a line whatever the node asks for:
        // its geometry is horizontal.
        let across = shaper
            .shape(
                node(),
                &TextContent {
                    value: "や".into()
                },
                &horizontal,
                constraints,
            )
            .width;
        let offset = shaper.horizontal_offset(
            node(),
            &TextContent {
                value: "やや".into(),
            },
            "や".len(),
            &vertical,
        );
        assert!(
            (offset - across).abs() < 0.01,
            "an editor offset is the horizontal advance {across}, got {offset}"
        );
    }

    #[test]
    fn host_font_empty_bytes_are_rejected() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            register_host_font_bytes(Vec::new()),
            Err(HostFontError::Empty)
        );
        assert_eq!(
            register_host_font_bytes(b"not-a-font".to_vec()),
            Err(HostFontError::Unrecognized)
        );
        let missing = register_host_font_file("/definitely/not/a/font.ttf");
        assert!(matches!(missing, Err(HostFontError::Io(_))));
    }

    fn first_glyph_advance(style: &ComputedStyle) -> f32 {
        NanaTextShaper::default()
            .shape(
                node(),
                &TextContent { value: "A".into() },
                style,
                TextShapeConstraints {
                    shaping: TextShaping::Advanced,
                    ..TextShapeConstraints::default()
                },
            )
            .width
    }

    /// Issue #41: an axis the face declares must reach shaping as itself, and
    /// an axis that is not `wght` must never be remapped onto weight.
    #[test]
    fn custom_variation_axes_change_outlines_without_becoming_wght() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("nana_text/fixtures/nana-wdth-bevl.ttf");
        assert!(
            register_host_font_bytes(bytes.to_vec()).unwrap() > 0,
            "variable-font fixture must load"
        );
        let used = shaped_face_families("NanaTestVF", "A");
        assert!(
            used.iter().any(|name| name == "NanaTestVF"),
            "shaper must hit NanaTestVF, used={used:?}"
        );
        let base = ComputedStyle {
            font_family: Some("NanaTestVF".into()),
            font_size: 20.0,
            font_weight: Some(400),
            ..ComputedStyle::default()
        };
        let with = |axis: [u8; 4], value: f32| ComputedStyle {
            font_variations: vec![FontVariationSetting::new(axis, value)],
            ..base.clone()
        };
        let narrow = first_glyph_advance(&with(*b"wdth", 50.0));
        let wide = first_glyph_advance(&with(*b"wdth", 200.0));
        let unbeveled = first_glyph_advance(&with(*b"BEVL", 0.0));
        let beveled = first_glyph_advance(&with(*b"BEVL", 100.0));
        assert!(
            wide > narrow + 1.0,
            "wdth must change advance, narrow={narrow} wide={wide}"
        );
        assert!(
            (beveled - unbeveled).abs() > 1.0,
            "BEVL must change outlines/advance, off={unbeveled} on={beveled}"
        );

        // The Runtime path end to end: the style field CSS
        // `font-variation-settings` lands in, a retained layout, and the face
        // instance its runs carry to the rasterizer's key. `XXXX` is not an
        // axis of this face and must be dropped, not become `wght`.
        let mut world = nana_ui_runtime::UiWorld::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let id = nana_ui_runtime::StableNodeId::new(1).unwrap();
        let mut queue = nana_ui_runtime::MutationQueue::new();
        queue.create(id, document, nana_ui_runtime::NodeKind::Text);
        queue.set_text(id, TextContent { value: "A".into() });
        let mut style = nana_ui_runtime::NodeStyle::default();
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.font_family = Some("NanaTestVF".into());
        layout.font_size = Some(20.0);
        layout.font_variation_settings = Some(vec![
            FontVariationSetting::new(*b"BEVL", 100.0),
            FontVariationSetting::new(*b"XXXX", 5.0),
        ]);
        queue.set_style(id, style);
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world
            .shape_text(&work.text, &mut NanaTextShaper::default())
            .unwrap();
        let (_, retained) = world
            .text_layout(id)
            .expect("a text node retains its layout");
        let instance = retained.runs.first().expect("one run").instance.clone();
        let coords = &instance.expect("the run names its face instance").coords;
        assert!(
            coords
                .iter()
                .any(|coord| coord.tag == *b"BEVL" && coord.value == 100.0),
            "BEVL reaches the instance the painter rasterizes: {coords:?}"
        );
        assert!(
            coords
                .iter()
                .all(|coord| coord.tag != *b"XXXX" && coord.tag != *b"wght"),
            "an axis the face lacks is dropped, and nothing becomes wght: {coords:?}"
        );
    }

    /// Issue #85: an animated axis is a real glyph variation. Every sample of
    /// a font-axis Motion track reaches the face instance the runs are shaped
    /// and rasterized with, and the advance moves with it — no compositor
    /// scale stands in for the variation.
    #[test]
    fn font_axis_motion_reshapes_with_each_sampled_instance() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("nana_text/fixtures/nana-wdth-bevl.ttf");
        assert!(register_host_font_bytes(bytes.to_vec()).unwrap() > 0);
        let mut world = nana_ui_runtime::UiWorld::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let id = nana_ui_runtime::StableNodeId::new(1).unwrap();
        let mut queue = nana_ui_runtime::MutationQueue::new();
        queue.create(id, document, nana_ui_runtime::NodeKind::Text);
        queue.set_text(id, TextContent { value: "AA".into() });
        let mut style = nana_ui_runtime::NodeStyle::default();
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.font_family = Some("NanaTestVF".into());
        layout.font_size = Some(20.0);
        layout.font_variation_settings = Some(vec![
            FontVariationSetting::new(*b"wdth", 50.0),
            FontVariationSetting::new(*b"BEVL", 0.0),
        ]);
        queue.set_style(id, style);
        queue
            .node(id, std::time::Duration::ZERO)
            .transition()
            .font_axis(*b"wdth", 200.0)
            .font_axis(*b"BEVL", 100.0)
            .duration(std::time::Duration::from_millis(100))
            .ease(nana_ui_runtime::Easing::Linear)
            .start();
        world.commit(queue).unwrap();
        let mut shaper = NanaTextShaper::default();
        let mut widths = Vec::new();
        for (ms, wdth, bevl) in [(0, 50.0, 0.0), (50, 125.0, 50.0), (100, 200.0, 100.0)] {
            world.advance_animations(std::time::Duration::from_millis(ms));
            let work = world.take_system_work();
            assert!(
                work.text.contains(&id),
                "{ms}ms: the sample schedules text work"
            );
            world.resolve_styles(&work.style).unwrap();
            world.shape_text(&work.text, &mut shaper).unwrap();
            let (_, retained) = world.text_layout(id).expect("retained layout");
            let coords = retained.runs[0]
                .instance
                .clone()
                .expect("the run names its face instance")
                .coords;
            let coord = |tag: [u8; 4]| {
                coords
                    .iter()
                    .find(|coord| coord.tag == tag)
                    .map(|coord| coord.value)
            };
            assert_eq!(coord(*b"wdth"), Some(wdth), "{ms}ms: {coords:?}");
            // The instance leaves out a coordinate at the axis default (0).
            assert_eq!(coord(*b"BEVL").unwrap_or(0.0), bevl, "{ms}ms: {coords:?}");
            assert!(
                coords.iter().all(|coord| coord.tag != *b"wght"),
                "{ms}ms: {coords:?}"
            );
            widths.push(retained.bounds.width);
            if ms == 50 {
                assert!(
                    world
                        .inspect_motion()
                        .iter()
                        .all(|entry| entry.ineffective_reason.is_none()),
                    "axes the face has take effect"
                );
            }
        }
        assert!(
            widths[0] + 1.0 < widths[1] && widths[1] + 1.0 < widths[2],
            "the advance follows the sampled wdth: {widths:?}"
        );
    }

    /// An animated axis no face of the text has is reported rather than
    /// passed over in silence, and never mapped onto another axis.
    #[test]
    fn an_animated_axis_the_face_lacks_is_reported() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("nana_text/fixtures/nana-wdth-bevl.ttf");
        assert!(register_host_font_bytes(bytes.to_vec()).unwrap() > 0);
        let mut world = nana_ui_runtime::UiWorld::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let id = nana_ui_runtime::StableNodeId::new(1).unwrap();
        let mut queue = nana_ui_runtime::MutationQueue::new();
        queue.create(id, document, nana_ui_runtime::NodeKind::Text);
        queue.set_text(id, TextContent { value: "A".into() });
        let mut style = nana_ui_runtime::NodeStyle::default();
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.font_family = Some("NanaTestVF".into());
        layout.font_size = Some(20.0);
        layout.font_variation_settings = Some(vec![FontVariationSetting::new(*b"XXXX", 0.0)]);
        queue.set_style(id, style);
        world.commit(queue).unwrap();
        let mut shaper = NanaTextShaper::default();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.shape_text(&work.text, &mut shaper).unwrap();

        let mut queue = nana_ui_runtime::MutationQueue::new();
        queue
            .node(id, std::time::Duration::ZERO)
            .transition()
            .font_axis(*b"XXXX", 100.0)
            .duration(std::time::Duration::from_millis(100))
            .ease(nana_ui_runtime::Easing::Linear)
            .start();
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.shape_text(&work.text, &mut shaper).unwrap();
        let entry = world
            .inspect_motion()
            .into_iter()
            .find(|entry| entry.property == nana_ui_runtime::AnimatableProperty::FontAxis(*b"XXXX"))
            .expect("the track is inspectable");
        assert!(entry.ineffective_reason.is_some(), "{entry:?}");
        assert!(
            !world
                .inspect_motion()
                .iter()
                .any(|entry| entry.property
                    == nana_ui_runtime::AnimatableProperty::FontAxis(*b"wght")),
            "nothing became wght"
        );
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn css_family_alias_shapes_loaded_face() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/fonts/NotoSansSC-Regular.ttf"),
        )
        .expect("bundled Noto Sans SC Regular");
        let added = register_host_font_face("Host Sans", bytes, Some(400), None);
        assert!(added > 0, "Noto bytes must load");
        let used = shaped_face_families("Host Sans", "H");
        assert!(
            used.iter().any(|name| name == "Host Sans"),
            "shaper must hit the CSS alias, used={used:?}"
        );
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn nowrap_ellipsis_keeps_exact_fit_and_truncates_narrow_labels() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut shaper = NanaTextShaper::default();
        let style = ComputedStyle {
            font_size: 12.0,
            font_weight: Some(600),
            font_family: Some("Noto Sans SC".into()),
            ..ComputedStyle::default()
        };
        let constraints = TextShapeConstraints {
            wrap: false,
            shaping: TextShaping::Advanced,
            ..TextShapeConstraints::default()
        };
        for label in ["未命名 1", "shade", "效果图", "fs_main"] {
            let text = TextContent {
                value: label.into(),
            };
            let natural = shaper.shape(node(), &text, &style, constraints).width;
            for width in [natural, natural * 0.5] {
                let shaped = shaper
                    .shape(
                        node(),
                        &text,
                        &style,
                        TextShapeConstraints {
                            max_width: Some(width),
                            ellipsis: true,
                            ..constraints
                        },
                    )
                    .width;
                if width == natural {
                    assert!(
                        (shaped - natural).abs() < 0.01,
                        "exact-fit {label:?}: {shaped} vs {natural}"
                    );
                } else {
                    assert!(
                        shaped <= width + 0.5 && shaped < natural,
                        "narrow {label:?}: {shaped} exceeds {width}"
                    );
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "bundled-fonts")]
    fn breadcrumb_segments_fit_wide_center_and_ellipsis_in_narrow_center() {
        let _font_test = FONT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        for center_width in [440.0, 90.0] {
            let mut context = nana_ui_runtime::AppContext::new();
            let document = nana_ui_runtime::DocumentId::new(1).unwrap();
            let breadcrumb = context
                .create_component(document, nana_ui_runtime::Breadcrumb::new())
                .unwrap();
            context
                .set_breadcrumb_items(
                    breadcrumb,
                    vec![
                        nana_ui_runtime::BreadcrumbItem::new("未命名 1")
                            .tone(nana_ui_runtime::BreadcrumbTone::Parent),
                        nana_ui_runtime::BreadcrumbItem::new("shade")
                            .tone(nana_ui_runtime::BreadcrumbTone::Current)
                            .interactive(true),
                    ],
                )
                .unwrap();
            let bar = context
                .create_component(
                    document,
                    nana_ui_runtime::AppTitleBar::new("Nana")
                        .center(breadcrumb.stable_id())
                        .center_width(center_width),
                )
                .unwrap();
            context.assemble_app_title_bar(bar).unwrap();

            let mut shaper = NanaTextShaper::default();
            let order = context.world().document_order(document);
            context.shape_text(&order, &mut shaper).unwrap();
            context
                .layout_document(document, nana_ui_runtime::LayoutViewport::new(800.0, 400.0))
                .unwrap();
            let reshaped = context
                .shape_text_for_layout(document, &mut shaper)
                .unwrap();
            if reshaped {
                context
                    .layout_document(document, nana_ui_runtime::LayoutViewport::new(800.0, 400.0))
                    .unwrap();
            }

            let segments = context
                .read(breadcrumb, |bar| bar.segment_nodes().to_vec())
                .unwrap();
            assert_eq!(segments.len(), 2);
            let mut truncated = false;
            for segment in segments {
                let label = context
                    .world()
                    .text(segment)
                    .expect("segment text")
                    .to_owned();
                let style = context
                    .world()
                    .computed_style(segment)
                    .expect("segment style")
                    .clone();
                let metrics = context
                    .world()
                    .text_metrics(segment)
                    .expect("segment metrics");
                let natural = shaper.shape(
                    segment,
                    &TextContent {
                        value: label.clone().into(),
                    },
                    &style,
                    TextShapeConstraints {
                        wrap: false,
                        shaping: TextShaping::Advanced,
                        ..TextShapeConstraints::default()
                    },
                );
                let bounds = context.world().layout_box(segment).expect("segment layout");
                if center_width == 440.0 {
                    assert!(
                        (metrics.width - natural.width).abs() < 0.5,
                        "short breadcrumb {label:?} must keep natural width: metrics={} natural={}",
                        metrics.width,
                        natural.width
                    );
                } else {
                    // A cut single line asks layout for its whole line
                    // (docs/reference/text-engine.md); what it paints is the retained
                    // layout, cut to the box.
                    assert!(
                        (metrics.width - natural.width).abs() < 0.5,
                        "narrow segment {label:?} must still ask for its whole line: metrics={} natural={}",
                        metrics.width,
                        natural.width
                    );
                    let (_, painted) = context
                        .world()
                        .text_layout(segment)
                        .expect("segment retains its layout");
                    assert!(
                        painted.bounds.width <= bounds.width + 0.5,
                        "narrow segment {label:?} must paint inside its box: painted={} box={}",
                        painted.bounds.width,
                        bounds.width
                    );
                    truncated |= painted
                        .overflow
                        .contains(nana_text::OverflowFlags::ELLIPSIZED)
                        && painted.bounds.width + 0.5 < natural.width;
                }
            }
            if center_width < 440.0 {
                assert!(
                    truncated,
                    "narrow breadcrumb center must shrink overflowing text"
                );
            }
        }
    }
}
